use std::{net::SocketAddr, path::Path};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{HeaderValue, Request},
};
use zz_web::{Gateway, GatewayConfig};

struct RunningGateway {
    address: SocketAddr,
    task: JoinHandle<std::io::Result<()>>,
}

impl RunningGateway {
    async fn start(assets: &Path, socket: &Path) -> Self {
        let gateway = Gateway::bind(GatewayConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            assets: assets.to_path_buf(),
            socket: socket.to_path_buf(),
        })
        .await
        .unwrap();
        Self {
            address: gateway.local_addr(),
            task: tokio::spawn(gateway.serve()),
        }
    }

    fn websocket_request(&self) -> Request<()> {
        let mut request = format!("ws://{}/ws", self.address)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "Origin",
            HeaderValue::from_str(&format!("http://{}", self.address)).unwrap(),
        );
        request
    }

    async fn http(&self, path: &str, host: &str) -> String {
        let mut stream = TcpStream::connect(self.address).await.unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }
}

impl Drop for RunningGateway {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn gateway_serves_assets_and_rejects_foreign_hosts_and_path_escape() {
    let scratch = tempfile::tempdir().unwrap();
    let assets = scratch.path().join("assets");
    std::fs::create_dir(&assets).unwrap();
    std::fs::write(assets.join("index.html"), "browser-client").unwrap();
    std::fs::write(assets.join("client.wasm"), b"wasm").unwrap();
    std::fs::write(scratch.path().join("secret"), "private").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(scratch.path().join("secret"), assets.join("escape")).unwrap();

    let server = RunningGateway::start(&assets, &scratch.path().join("absent.sock")).await;
    let host = server.address.to_string();
    let index = server.http("/", &host).await;
    assert!(index.starts_with("HTTP/1.1 200"));
    assert!(index.ends_with("browser-client"));
    assert!(index.contains("cross-origin-embedder-policy: require-corp"));
    let wasm = server.http("/client.wasm", &host).await;
    assert!(wasm.contains("content-type: application/wasm"));
    assert!(
        server
            .http("/", "attacker.example")
            .await
            .starts_with("HTTP/1.1 403")
    );
    for path in ["/../secret", "/%2e%2e/secret", "/%5csecret", "/escape"] {
        assert!(
            server.http(path, &host).await.starts_with("HTTP/1.1 404"),
            "accepted {path}"
        );
    }

    assert!(
        Gateway::bind(GatewayConfig {
            bind: "0.0.0.0:0".parse().unwrap(),
            assets,
            socket: scratch.path().join("absent.sock"),
        })
        .await
        .is_err()
    );
}

#[tokio::test]
async fn websocket_requires_an_exact_same_origin_before_connecting_the_daemon() {
    let scratch = tempfile::tempdir().unwrap();
    let server = RunningGateway::start(scratch.path(), &scratch.path().join("absent.sock")).await;
    for origin in [
        None,
        Some("null"),
        Some("http://attacker.example"),
        Some("http://localhost:1"),
    ] {
        let mut request = server.websocket_request();
        request.headers_mut().remove("Origin");
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("Origin", HeaderValue::from_static(origin));
        }
        let result = tokio_tungstenite::connect_async(request).await;
        assert!(matches!(
            result,
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == 403
        ));
    }
    let result = tokio_tungstenite::connect_async(server.websocket_request()).await;
    assert!(matches!(
        result,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == 503
    ));
}

#[cfg(unix)]
mod daemon {
    use super::*;
    use std::{path::PathBuf, thread, time::Duration};

    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
    use zz_client::{ClientCore, Outbound};
    use zz_daemon::{CommandClient, Daemon};
    use zz_protocol::{
        ClientHello, ClientInstanceId, ClientKind, CommandInvocation, CommandRequest,
        CommandResponse, InputMessage, PROTOCOL_VERSION, ProtocolMessage, decode_protocol_frame,
        encode_protocol_message,
    };
    use zz_terminal::TerminalColorScheme;

    struct DaemonFixture {
        socket: PathBuf,
        thread: Option<thread::JoinHandle<()>>,
        _scratch: tempfile::TempDir,
    }

    impl DaemonFixture {
        fn start() -> Self {
            let scratch = tempfile::Builder::new()
                .prefix("zz-web-")
                .tempdir_in("/tmp")
                .unwrap();
            let socket = scratch.path().join("d.sock");
            let daemon = Daemon::new(&socket).without_user_config();
            let (ready_send, ready_recv) = std::sync::mpsc::channel();
            let thread = thread::spawn(move || {
                daemon
                    .run_foreground_with_ready(|_| ready_send.send(()).unwrap())
                    .unwrap();
            });
            ready_recv.recv_timeout(Duration::from_secs(30)).unwrap();
            let mut commands = CommandClient::connect(&socket).unwrap();
            commands
                .execute(CommandInvocation::new(
                    "new-session",
                    [
                        "-d",
                        "-s",
                        "web-test",
                        "printf 'gateway-ready\\r\\n'; exec /bin/cat",
                    ],
                ))
                .unwrap();
            Self {
                socket,
                thread: Some(thread),
                _scratch: scratch,
            }
        }
    }

    impl Drop for DaemonFixture {
        fn drop(&mut self) {
            if let Ok(mut commands) = CommandClient::connect(&self.socket) {
                let _ = commands.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    type BrowserSocket = WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

    async fn send(socket: &mut BrowserSocket, message: &ProtocolMessage) {
        socket
            .send(Message::Binary(
                encode_protocol_message(message).unwrap().into(),
            ))
            .await
            .unwrap();
    }

    async fn receive(socket: &mut BrowserSocket, core: &mut ClientCore) -> ProtocolMessage {
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(15), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        else {
            panic!("expected a binary zz frame");
        };
        let message = decode_protocol_frame(&frame).unwrap();
        core.handle_message(message.clone());
        while let Some(Outbound::RequestFull(pane)) = core.poll_outbound() {
            send(socket, &ProtocolMessage::RequestFull { pane }).await;
        }
        while core.poll_event().is_some() {}
        message
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn browser_handshake_commands_terminal_input_and_disconnect_use_the_existing_daemon() {
        let fixture = DaemonFixture::start();
        let assets = tempfile::tempdir().unwrap();
        let server = RunningGateway::start(assets.path(), &fixture.socket).await;
        let (mut socket, _) = tokio_tungstenite::connect_async(server.websocket_request())
            .await
            .unwrap();
        send(
            &mut socket,
            &ProtocolMessage::ClientHello(ClientHello {
                protocol_version: PROTOCOL_VERSION,
                client_instance_id: ClientInstanceId(0x0062_726f_7773_6572),
                kind: ClientKind::Interactive,
                device_name: Some("Browser gateway test".into()),
                capabilities: vec![
                    ClientHello::CLIENT_TERMINAL_CAPABILITY.into(),
                    ClientHello::CLIENT_UTF8_CAPABILITY.into(),
                ],
                color_scheme: Some(TerminalColorScheme::Dark),
                origin: None,
                working_directory: None,
                environment: Vec::new(),
                process_id: 0,
            }),
        )
        .await;
        let mut core = ClientCore::new();
        assert!(matches!(
            receive(&mut socket, &mut core).await,
            ProtocolMessage::ServerHello(_)
        ));
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        send(
            &mut socket,
            &ProtocolMessage::Attach {
                session: "web-test".into(),
            },
        )
        .await;
        while core.attached_session().is_none() {
            assert!(std::time::Instant::now() < deadline, "attachment timed out");
            receive(&mut socket, &mut core).await;
        }
        let session = core
            .snapshot()
            .sessions
            .iter()
            .find(|session| Some(session.id) == core.attached_session())
            .unwrap();
        let pane = *session.windows[0].panes.keys().next().unwrap();
        send(
            &mut socket,
            &ProtocolMessage::Input(InputMessage::ResizeTerminal {
                pane,
                columns: 60,
                rows: 12,
                cell_width_px: 8,
                cell_height_px: 16,
            }),
        )
        .await;
        send(
            &mut socket,
            &ProtocolMessage::Input(InputMessage::Text {
                pane,
                text: "browser-roundtrip\n".into(),
            }),
        )
        .await;
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "terminal input timed out"
            );
            receive(&mut socket, &mut core).await;
            if core.viewport(pane).is_some_and(|viewport| {
                let mut text = String::new();
                for cell in viewport.cells.iter() {
                    viewport.push_glyph(*cell, &mut text);
                }
                viewport.columns == 60 && viewport.rows == 12 && text.contains("browser-roundtrip")
            }) {
                break;
            }
        }

        send(
            &mut socket,
            &ProtocolMessage::CommandRequest(CommandRequest {
                request_id: 7,
                command: CommandInvocation::new("display-message", ["-p", "#{session_name}"]),
                prepared: false,
            }),
        )
        .await;
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "command reply timed out"
            );
            if let ProtocolMessage::CommandResponse(CommandResponse::Success {
                request_id: 7,
                output,
                ..
            }) = receive(&mut socket, &mut core).await
            {
                assert_eq!(output.as_str(), "web-test");
                break;
            }
        }
        socket.close(None).await.unwrap();
        drop(socket);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut commands = CommandClient::connect(&fixture.socket).unwrap();
            let clients = commands
                .execute(CommandInvocation::new(
                    "list-clients",
                    ["-F", "#{client_name}"],
                ))
                .unwrap();
            if clients.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "browser attachment remained after close: {clients}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
