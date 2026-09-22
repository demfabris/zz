use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};
use zz_daemon::{AskpassPromptKind, AskpassReply, Endpoint, InteractiveClient, SshPrompts};
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
    client: Arc<Mutex<Option<Arc<InteractiveClient>>>>,
    cancelled: Arc<AtomicBool>,
}

impl Connection {
    pub fn connect(endpoint: String, session: String) -> Self {
        let (tx, events) = mpsc::sync_channel(64);
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
                    InteractiveClient::connect_terminal_surface_endpoint_with_prompts(
                        &endpoint,
                        TerminalColorScheme::Dark,
                        Some(prompts),
                    )
                    .map_err(|error| error.to_string())?,
                );
                {
                    let mut slot = shared_client.lock().unwrap();
                    if cancel.load(Ordering::Acquire) {
                        let _ = connected.shutdown();
                        return Ok(());
                    }
                    *slot = Some(connected.clone());
                }
                connected
                    .attach(session.clone())
                    .map_err(|error| error.to_string())?;
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
            client,
            cancelled,
        }
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
