use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};
use zz_daemon::{
    AskpassPromptKind, AskpassReply, DaemonError, Endpoint, EndpointError, InteractiveClient,
    SshPrompts,
};
use zz_protocol::ProtocolMessage;
use zz_terminal::TerminalColorScheme;

pub struct Prompt {
    pub kind: AskpassPromptKind,
    pub text: String,
    pub echo: bool,
    pub reply: Sender<AskpassReply>,
}

pub enum Event {
    Connected(Arc<InteractiveClient>),
    Message(Box<ProtocolMessage>),
    Prompt(Prompt),
    Failed(String),
}

pub struct Connection {
    pub events: Receiver<Event>,
    pub ready: Option<futures::channel::mpsc::UnboundedReceiver<()>>,
    client: Arc<Mutex<Option<Arc<InteractiveClient>>>>,
    cancelled: Arc<AtomicBool>,
}

impl Connection {
    #[must_use]
    pub fn connect(endpoint: String, session: Option<String>, native_ui: bool) -> Self {
        let (tx, events) = mpsc::sync_channel(64);
        let (ready_tx, ready) = futures::channel::mpsc::unbounded();
        let tx = Notifying {
            events: tx,
            ready: ready_tx,
        };
        let client = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let shared_client = client.clone();
        let cancel = cancelled.clone();
        std::thread::spawn(move || {
            let run = || -> Result<(), String> {
                let endpoint = Endpoint::parse(&endpoint).map_err(|error| error.to_string())?;
                let prompts = SshPrompts::new(PathBuf::new(), {
                    let tx = tx.clone();
                    let cancel = cancel.clone();
                    move |prompt| {
                        if cancel.load(Ordering::Acquire) {
                            return AskpassReply::Cancel;
                        }
                        let (reply, answer) = mpsc::channel();
                        if tx
                            .send(Event::Prompt(Prompt {
                                kind: prompt.kind(),
                                text: prompt.text().to_owned(),
                                echo: prompt.echo(),
                                reply,
                            }))
                            .is_err()
                        {
                            return AskpassReply::Cancel;
                        }
                        loop {
                            match answer.recv_timeout(std::time::Duration::from_millis(100)) {
                                Ok(reply) => return reply,
                                Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    return AskpassReply::Cancel;
                                }
                                Err(mpsc::RecvTimeoutError::Timeout)
                                    if cancel.load(Ordering::Acquire) =>
                                {
                                    return AskpassReply::Cancel;
                                }
                                _ => {}
                            }
                        }
                    }
                });
                let connected = Arc::new(
                    InteractiveClient::connect_endpoint_with_prompts_and_attach(
                        &endpoint,
                        Some(TerminalColorScheme::Dark),
                        Some(prompts),
                        &[],
                        session.map(zz_protocol::AttachOperation::Session),
                        !native_ui,
                    )
                    .map_err(|error| failure_reason(&error))?,
                );
                {
                    let mut slot = shared_client.lock().unwrap();
                    if cancel.load(Ordering::Acquire) {
                        let _ = connected.shutdown();
                        return Ok(());
                    }
                    *slot = Some(connected.clone());
                }
                if tx.send(Event::Connected(connected.clone())).is_err() {
                    return Ok(());
                }
                while !cancel.load(Ordering::Acquire) {
                    let message = connected.recv().map_err(|error| error.to_string())?;
                    if tx.send(Event::Message(Box::new(message))).is_err() {
                        break;
                    }
                }
                Ok(())
            };
            if let Err(error) = run() {
                let _ = tx.send(Event::Failed(error));
            }
            if let Some(client) = shared_client.lock().unwrap().take() {
                let _ = client.shutdown();
            }
        });
        Self {
            events,
            ready: Some(ready),
            client,
            cancelled,
        }
    }
}

#[derive(Clone)]
struct Notifying {
    events: mpsc::SyncSender<Event>,
    ready: futures::channel::mpsc::UnboundedSender<()>,
}

impl Notifying {
    fn send(&self, event: Event) -> Result<(), mpsc::SendError<Event>> {
        self.events.send(event)?;
        let _ = self.ready.unbounded_send(());
        Ok(())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(client) = self.client.lock().unwrap().take() {
            let _ = client.shutdown();
        }
    }
}

fn failure_reason(error: &DaemonError) -> String {
    EndpointError::find(error)
        .and_then(EndpointError::ssh_reason)
        .unwrap_or_else(|| error.to_string())
}
